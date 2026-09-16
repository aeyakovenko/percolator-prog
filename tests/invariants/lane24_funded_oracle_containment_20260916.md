# Lane 24: Funded Oracle Containment Across Resolution

Eight public LiteSVM worlds pass. Cold-admin oracle replacement commutes with
market resolution while funded backing and distinct insurance roles retain
their recipients. Cold-admin burn leaves bounded unsigned reserve recovery.
No public-route LoF, persistent DoS or CU bug was found; this is tests/docs only.
**Row 416 remains OPEN/missing; INV-005 remains REFUTED_CURRENT.** No aggregate
invariant or benchmark disposition is promoted.

## Isolation and ownership

- Read-only source: `/tmp/percolator-astra-invariant-cycle-20260915-run`, branch
  `codex/astra-invariant-cycle-20260915`, clean at the initial check.
- Base: `d64f049005847848b095b3b8b2d21318d0504296`.
- Private clone: `/tmp/percolator-lane24-funded-oracle-containment-20260916`.
- Local branch: `codex/lane24-funded-oracle-containment-20260916`; no push.
- Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
- All edits and Git mutations target the private clone. Builds, logs, temporary
  files and artifacts use private `/dev/shm/lane24-20260916-*` paths. The matcher
  fixture's ignored `target` symlink points to the private matcher build.
- No cached build tree was copied. Wrapper and authenticated matcher SBFs were
  freshly built with default features, platform-tools v1.52 and locked offline
  dependencies. The host harnesses were also built in a fresh private target.

Changed files are the new
[Rust owner](cu/inv_005_cold_oracle_resolved_containment.rs), its three-line mount
in [funded oracle succession](cu/inv_005_funded_oracle_succession.rs), the short
[README](README.md) entry and this report. Production, Cargo manifests/locks and
all machine status TSVs remain unchanged.

## Gap and non-overlap

The survey covered row 416's surrounding evidence, the insurance-funded oracle
owner (including Lane 14), backing-funded oracle succession, shutdown oracle ABA,
cold-admin ABA/burn, retained insurance management, generated funded-role epochs,
zero-role transitions, the funded-role source guard and the lane reports.
`scripts/loop.md` supplies the public-reachability and bug-classification rules.
Required domain authorities cannot be zeroed; only the cold admin can be burned.
The selected product exercises that supported transition after resolution.

| Existing owner / lane | Difference in Lane 24 |
| --- | --- |
| Lane 1 retained policy and oracle role close | No trader, CPI close, fee policy or retained trade envelope; the retained instructions here are role management and reserve payouts. |
| Lanes 2 and 8 late receipts and Recovery cleanup | No claims, overdue backing, native quote rail, portfolio cleanup or liquidity shortage; the lifecycle axis is explicit Live-to-Resolved. |
| Lanes 3, 9, 18 and 21 mixed debt attribution | No funding, pending losses, bankruptcy, cross-asset debt, expired close or native retirement. All 76 reserve atoms have input-derived recipients. |
| Lanes 4 and 5 observations, Lane 6 limits, Lanes 7 and 16 maximum-shape progress, Lane 19 source capacity | No report renewal, limit boundary, backlog, latent source or maximum-shape product. CU is only a bound on the authority/recovery path. |
| Lane 10 paid insurance beneficiary succession | Beneficiary and operator stay distinct and fixed throughout. Here oracle replacement and resolution change which existing insurance role can receive value, followed by cold-admin burn; no beneficiary handoff or slab closure. |
| Lane 11 consumed backing / earned-fee boundary | Both backing domains contain fresh principal with no consumed, impaired or earned stock. Resolution and disjoint live/terminal insurance recipients are the new axes. |
| Lanes 12 and 15 Hybrid reward provenance | No Hybrid feed, liquidation, keeper reward or competing exposed recipient. |
| Lane 13 pending users at provider expiry; Lanes 17 and 20 receipt/fee expiry | No users, receipts, fee credit, provider expiry, missing custody or terminal beneficiary succession. |
| Lane 14 open AuthMark/Hybrid oracle round trip | No open position, report movement, PnL or oracle return. This product crosses resolution order, mixed funded backing/insurance, the live/terminal recipient switch and cold-admin burn. Lane 14 is rerun unchanged as a control. |
| Original insurance/backing cold-oracle selectors | They stay Live/Active and test one funded reserve class. Here both classes coexist through resolution, with separate insurance recipients and unsigned terminal transfers. |
| Existing shutdown oracle ABA | It stays market-Live, coalesces insurance beneficiary/operator and rotates by incumbent consent. Here cold/new/admin sign without any funded holder, the market becomes Resolved, the cold role is burned and recovery is unsigned. |
| Cold-admin ABA/burn and Scope T generated funded-role epochs | Those flat Live histories do not cross the resolved recipient switch or combine oracle replacement/resolution rollback with unsigned reserve delivery. |

## Public history and independent oracle

The product is two target assets x two oracle coholder shapes x two resolution
orders. Actor 0 is always the oracle/backing provider and also either the
insurance beneficiary or operator. Actor 1 holds the other insurance role.
Cold admin, incoming oracle, market admin and fee payer are distinct keys.

System/ATA/SPL/wrapper instructions create every economic account and stock.
Only program loading, signer SOL and Clock are environmental inputs. No
program-owned Account is injected or edited; no engine transition constructs
state. The test reuses the public SPL market fixture and the existing funded
oracle transaction checker, instruction builder and profile reader.

Backing principal is 17/29 atoms in the target's long/short domains. Insurance
is 13/17. SPL mint authority is revoked at supply 76. The sibling asset has no
funding, and its profile and all control sequences remain exact sentinels.
There are no portfolios, PnL, positions, liens, insurance spend or earnings.

1. The live operator withdraws five atoms, leaving insurance budgets 8/17.
   The insurance debit advances only the target authority epoch.
2. Each funded holder's self-handoff is independently simulated at the current
   epoch and retained. These are retained instruction bytes, re-signed on
   submission, not detached signed transaction envelopes.
3. Cold-admin oracle replacement and market resolution execute in either order,
   followed by a three-atom unsigned backing payout. Each of the three funded
   cold-admin takeover suffixes rejects `EngineLockActive` at instruction 5.
   Logs prove all three wrapper prefixes and the SPL transfer completed; full
   Account comparison restores resolution, authority epoch/profile and custody.
   The exact prefix then commits. The only signers are payer, cold admin,
   incoming oracle and market admin, with no funded holder signature.
4. Both the retained live payout and its current-epoch version reject
   `InvalidTokenAccount`: the operator's ATA is not the terminal beneficiary's.
   A cold oracle-management request simulates successfully immediately before
   cold-admin burn. The burn advances the target epoch and zeros only asset_admin.
5. A second three-atom terminal backing payout precedes three stale retained
   funded self-handoffs (`EngineStale`), retained/current-epoch burned-cold
   requests (`Unauthorized`), and three new-oracle funded takeovers
   (`Unauthorized`). Every suffix restores the completed SPL transfer. Runtime
   signer promotion may sign the backing prefix when its holder signs a retained
   self-handoff; the committed retry uses only the payer signature.
6. The same payout prefix commits unsigned. All three renewed incumbent
   self-handoffs succeed after cold burn and advance the epoch individually.
   Unsigned insurance pays the remaining 25 atoms to the beneficiary and consumes
   one authority epoch. Two unsigned backing withdrawals pay the remaining 11/29
   atoms to the provider without advancing that epoch. The vault ends at zero.

The expected recipient book starts from the inputs, not measured payout deltas:
provider 46, live operator 5, terminal beneficiary 25, cold admin 0, new oracle 0.
Thus actor-indexed outcomes are `[51,25,0,0]` or `[71,5,0,0]`, according to
coholding, and must be identical across both resolution orders. Per-domain stock,
zero spent/earned/liens, insurance aggregates, wrapper/SPL custody, fixed supply,
complete target profiles/sequences and sibling sentinels are checked. The shared
transaction checker verifies packet size, exact signer sets, payer signature
fees, complete rejection frames and unchanged undeclared accounts; successful
SPL writes may change only token amounts.

## Commands and results

Run from the private clone. All logs below are under
`/dev/shm/lane24-20260916-logs/`.

```sh
git clone --no-hardlinks --single-branch --branch codex/astra-invariant-cycle-20260915 /tmp/percolator-astra-invariant-cycle-20260915-run /tmp/percolator-lane24-funded-oracle-containment-20260916
git switch -c codex/lane24-funded-oracle-containment-20260916
mkdir -p /dev/shm/lane24-20260916-logs /dev/shm/lane24-20260916-sbf /dev/shm/lane24-20260916-host /dev/shm/lane24-20260916-tmp
env CARGO_TARGET_DIR=/dev/shm/lane24-20260916-sbf CARGO_BUILD_JOBS=4 TMPDIR=/dev/shm/lane24-20260916-tmp RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/lane24-20260916-sbf/deploy -- --locked > /dev/shm/lane24-20260916-logs/sbf.log 2>&1
env CARGO_TARGET_DIR=/dev/shm/lane24-20260916-matcher CARGO_BUILD_JOBS=4 TMPDIR=/dev/shm/lane24-20260916-tmp RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --manifest-path tests/fixtures/auth_matcher/Cargo.toml --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/lane24-20260916-matcher/deploy -- --locked > /dev/shm/lane24-20260916-logs/matcher-build.log 2>&1
ln -s /dev/shm/lane24-20260916-matcher tests/fixtures/auth_matcher/target

export CARGO_TARGET_DIR=/dev/shm/lane24-20260916-host
export CARGO_BUILD_JOBS=6 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export TMPDIR=/dev/shm/lane24-20260916-tmp
export PERCOLATOR_FUZZ_SBF=/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so
cargo test --locked --offline --test v16_cu --test v16_program_fuzz_regressions --no-run > /dev/shm/lane24-20260916-logs/host-build.log 2>&1
cargo test --locked --offline --test v16_cu inv_005_authority_incarnation_binding::funded_oracle_succession::cold_oracle_resolved_containment::v16_program_cold_oracle_resolution_and_admin_burn_preserve_split_funded_recipients -- --exact --nocapture --test-threads=1 > /dev/shm/lane24-20260916-logs/new-final.log 2>&1
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_005_authority_incarnation_binding::funded_oracle_succession::cold_oracle_insurance_containment::v16_program_cold_oracle_replacement_preserves_insurance_funded_coholder \
  inv_005_authority_incarnation_binding::funded_oracle_succession::cold_oracle_funded_containment::v16_program_cold_oracle_replacement_preserves_funded_coholder_and_atomic_prefix \
  inv_005_authority_incarnation_binding::funded_oracle_succession::cold_oracle_insurance_containment::v16_program_funded_insurance_coholders_preserve_open_positions_through_oracle_round_trip \
  inv_005_authority_incarnation_binding::v16_program_funded_role_guard_and_oracle_handoff_are_source_complete \
  > /dev/shm/lane24-20260916-logs/controls.log 2>&1
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_dated_open_security_finding_benchmark_is_non_overclaiming \
  inv_079_public_reachability_evidence::v16_public_instruction_coverage_registry_matches_production_roster \
  inv_079_public_reachability_evidence::v16_program_invariant_harnesses_are_test_free_roots \
  inv_079_public_reachability_evidence::v16_public_trace_schema_detects_out_of_band_economic_mutation \
  > /dev/shm/lane24-20260916-logs/inv079.log 2>&1
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_005_funded_oracle_succession.rs tests/invariants/cu/inv_005_cold_oracle_resolved_containment.rs
git diff --check
git diff --cached --check
git diff --exit-code d64f049005847848b095b3b8b2d21318d0504296 -- src Cargo.toml Cargo.lock tests/invariants/open_findings.tsv tests/invariants/invariant_status.tsv tests/invariants/coverage_reopenings.tsv
git show --format= --check HEAD
sha256sum /dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so /dev/shm/lane24-20260916-matcher/deploy/auth_matcher.so
```

| Verification | Result |
| --- | --- |
| Fresh wrapper / matcher SBF builds | PASS, exit 0 |
| New exact selector | PASS, 1/1, eight worlds, 3.60 seconds |
| New rollback evidence | 104 exact rejections; 88 completed SPL transfers restored |
| New positive continuations | Eight oracle replacements, eight resolutions, eight cold burns, 24 current funded self-handoffs, 40 unsigned payouts, eight live insurance payouts |
| New maximum measured CU | 64,969 / 300,000; covers measured management/recovery transactions, not setup or simulations |
| Four nearby controls | PASS, 4/4, 15.22 seconds; backing peak 56,001, insurance peak 54,564, unchanged Lane 14 peak 136,165 |
| Six selected INV-079 guards | PASS, 6/6, 1.01 seconds before the docs-only addition; final rerun recorded in the same log |
| Touched Rust formatting, Git whitespace, protected-file diff | PASS |

The initial host compile found one Rust borrow error in the new test, corrected
by binding the vault key before passing a mutable environment reference. The
first runtime version passed 1/1 (96 rollbacks, peak 42,699), then was strengthened
to remove the funded holder's transaction signature from the core prefix and
add the current-epoch burned-cold control. The final runtime version above passed.
No failing economic test or red/green production-fix claim arose. Initial host
compile output is retained in `host-build.log`; the first runtime log is
`new-initial.log`. All runtime tests are the exact scoped selectors listed above.

Artifact SHA-256:

| Artifact | SHA-256 |
| --- | --- |
| `/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so` | `e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f` |
| `/dev/shm/lane24-20260916-matcher/deploy/auth_matcher.so` | `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93` |

## Remaining limits

This is a finite classic-SPL, AuthMark-configured, same-slot reserve-only product.
It does not cover open exposure, changing observations, Hybrid feeds, fees,
funding, earned/valid/impaired/consumed backing, spent insurance, telemetry
succession, Recovery, DrainOnly, arbitrary authority histories, signed-envelope
retention, native/secondary quote rails, maximum shapes, portfolio deletion or
slab retirement. Burned cold authority is not restored; required domain roles
remain nonzero. Funding and beneficiary identities stay fixed. These limits
leave row 416 OPEN/missing and all invariant machine statuses unchanged.
