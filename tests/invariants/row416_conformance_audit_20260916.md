# Row 416: Existing INV-005 Conformance Evidence

Decision: the requested bounded authority-incarnation and funded-role containment
slice already has executable public LiteSVM coverage. Reuse that evidence instead
of adding another test with the same assertions. **Row 416 remains `missing` in
`open_findings.tsv` and OPEN in `coverage_reopenings.tsv`; INV-005 remains
`REFUTED_CURRENT`.** This audit changes only documentation and ledger commentary.

## Evidence Boundary

The reviewed inventory includes `cu/inv_005_*`, the INV-005 README entries, and
the existing row-416 [terminal](row416_terminal_oracle_role_20260916.md),
[disabled-role](row416_disabled_role_restoration_20260916.md), and
[lien-retirement](row416_lien_release_role_boundary_20260916.md) reports.
The five selectors below directly cover this slice and were rerun unchanged.

The deployed `asset_authority_role_has_funded_value_view` treats oracle and
asset-admin roles as having no attributed stock. Consequently, cold admin plus
incoming oracle may replace the oracle without the outgoing coholder signing.
The public tests explicitly commit that replacement. Their economic assertion
is that it cannot acquire the separately funded backing or insurance role;
their incarnation assertion is that restoration cannot revive retained consent.
A source predicate check alone or an unauthorized-signer rejection would not
establish those properties.

| Public test owner | Observed assertion and result |
| --- | --- |
| [Fresh backing coholder](cu/inv_005_cold_oracle_funded_containment.rs) | Four asset/mark histories. A seven-atom user payout, cold-signed oracle replacement and accepted report execute before funded backing takeover rejects `EngineLockActive`. Exact rollback restores all three; the identical prefix then commits. Incumbent receives 17 + 29 backing atoms, user receives 23, other actors receive zero; frozen supply is 69. Four rollbacks; peak 51,501 CU. |
| [Insurance coholder](cu/inv_005_cold_oracle_insurance_containment.rs) | Four asset/mark histories. A five-atom payout/oracle/report prefix executes before funded insurance-beneficiary takeover rejects `EngineLockActive`. Retry preserves both incumbent insurance roles and pays the incumbent 13 + 17 atoms, user 23, others zero; frozen supply is 53. Four rollbacks; peak 51,564 CU. |
| [Terminal envelopes](cu/inv_005_terminal_oracle_role.rs) | Four asset/role histories. Prevalidated, unchanged signed payout/handoff/close envelopes remain stale after role and market-authority A-to-B-to-A. Completed SPL and closure prefixes roll back; current terminal bundles pay 100 atoms to the incumbent, 23 to the peer, and only seven donated atoms to market authority. Four complete closures, 30 rollbacks; peak 134,276 CU. |
| [Burned-admin restoration](cu/inv_005_disabled_role_restoration.rs) | Four admin-schedule/mark-mode histories. Required roles reject zero; burned admin cannot self-restore. Public retirement/reactivation restores A in a new generation with repeated authority/control counters, but retained signed reports, management and payouts reject generation mismatch in Live and Resolved. Current controls and owner payouts remain valid. 104 rollbacks; peak 76,824 CU. |
| [Lien retirement/refunding](cu/inv_005_lien_release_role_boundary.rs) | Four asset/source-side histories. Funded takeover rejects while impaired liens remain, including after position closure. Public final normalization admits empty-role takeover while a 120-atom claim survives. The same prevalidated management bytes reject after opposite-side refunding, including its last atom, and succeed after full withdrawal without an intervening epoch change. 24 rollbacks; peak setup/progress, rollback, continuation CU: 466,278 / 363,599 / 361,808. |

These tests use public System account creation, SPL mint/account initialization,
canonical ATAs, and Percolator initialization, deposits, top-ups, management,
trades/cranks and payouts. The common market builder is
`inv018_public_spl_market_with_params`; lien setup uses `liened_world`.
Economic state is not installed by mutating program-owned bytes. Program loading,
signer SOL and Clock advancement are environment inputs. Rejection helpers verify
signatures, errors, completed prefixes and full Account frames, allowing only the
fee payer's exact signature fee. Input-derived stock and recipient books check
actual SPL custody; simulated controls do not count as committed payouts.

Fresh-coholder and lien tests retain instruction bytes and sign submissions;
terminal and disabled-role tests additionally retain immutable signed envelopes.
This distinction is necessary when using the evidence for incarnation claims.

The passing finite histories do not independently discover the historical
privileged funded-oracle finding or refute its impact, so neither
`independent-discovery` nor `nonqualifying` is justified by this audit. In
particular, they do not assert that funding prevents oracle replacement, or
establish containment for arbitrary malicious prices, role combinations,
positions/claims, external/Hybrid feeds, native/secondary custody or maximum
shapes. Other existing INV-005 tests retain their own scope; no broader suite
result is claimed. This slice is executable, not blocked by a dependency.

## Reproduction

- Base branch: `codex/astra-invariant-cycle-20260915`.
- Base commit: `5c870324ae3241807423e237abfa1eaa8145b353`.
- Isolated branch: `codex/pr135-row416-inv005-20260916-H8fZto`.
- Worktree: `/dev/shm/pr135-row416-inv005-20260916-H8fZto/worktree`.
- Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
- SBF: private copy of `/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so`.
- SBF SHA-256: `e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f`.

The SBF is the same cached default-feature artifact recorded by the three prior
row-416 reports; no SBF rebuild is claimed. Production sources and Cargo files
match their recorded baseline `d809e9a563d9b8bf38f32648b32a15d75f526ec8` exactly.
The host dependency cache was copied without hardlinks from
`/dev/shm/percolator-row416-oracle-funded-target/debug` into a private target;
Cargo recompiled this worktree's `v16_cu` binary before running the selectors.
No matcher artifact is needed for these five tests.

Run from the worktree:

```sh
export CARGO_TARGET_DIR=/dev/shm/pr135-row416-inv005-20260916-H8fZto/target
export TMPDIR=/dev/shm/pr135-row416-inv005-20260916-H8fZto/tmp
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/dev/shm/pr135-row416-inv005-20260916-H8fZto/artifacts/percolator_prog.so

cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_005_authority_incarnation_binding::funded_oracle_succession::cold_oracle_funded_containment::v16_program_cold_oracle_replacement_preserves_funded_coholder_and_atomic_prefix \
  inv_005_authority_incarnation_binding::funded_oracle_succession::cold_oracle_insurance_containment::v16_program_cold_oracle_replacement_preserves_insurance_funded_coholder \
  inv_005_authority_incarnation_binding::cold_admin_handoff_scope::terminal_oracle_role::v16_program_retained_terminal_envelopes_bind_funded_oracle_roles_and_slab_closure \
  inv_005_authority_incarnation_binding::cold_admin_handoff_scope::disabled_role_restoration::v16_program_burned_admin_restoration_keeps_signed_oracle_and_funded_requests_stale \
  inv_005_authority_incarnation_binding::impaired_backing_containment::lien_release_role_boundary::v16_program_lien_retirement_reopens_cold_role_takeover_only_until_refunding
```

Result: **5 passed, 0 failed, 1,476 filtered out**, 10.69 seconds, covering
20 histories and 166 exact rollback checks. Every measured call stayed within
its existing CU assertion; keypair/PDA variation can change measurements.
Output is retained in the private `tmp/focused.log`. No unfiltered or unrelated
test suite was run. The source checkout at `/home/anatoly/percolator-prog` was
not edited. No production, Rust test, dependency or unrelated invariant changes
are included.
