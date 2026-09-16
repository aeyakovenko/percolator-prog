# Lane 25: funded oracle replacement across open lien impairment

## Scope and provenance

Target: `open_findings.tsv` row 416, `LoF PRIVILEGED INV-005 missing`,
`[PRIVILEGED LoF] Prevent cold-admin takeover of funded oracle`.

Base: `63cc6f284674f31187430f52d43a201b21b95d50`, from
`codex/astra-invariant-cycle-20260915` in the requested source checkout
`/tmp/percolator-astra-invariant-cycle-20260915-run`.
Independent clone: `/dev/shm/percolator-row416-funded-succession-20260916`.
Branch: `codex/row416-funded-succession-20260916`.
The initial `/tmp` clone failed for lack of root-filesystem space; the successful
clone uses its own Git objects and worktree on `/dev/shm`.

Only `tests/invariants/**` changes. Production, dependencies, fixtures and every
TSV disposition remain unchanged. No open fix, remote PR patch or holdout exploit
was consulted or merged. The coverage derives from the local README, existing
INV-005 witnesses, public instruction helpers and pinned engine accounting.

The owner is
`cu/inv_005_impaired_backing_containment.rs::cold_oracle_impaired_containment`.
The parent's public setup and expiry prefix are extracted into helpers; the
original two-world selector retains its assertions and original crank schedule.

## Non-overlap

| Existing coverage | Missing composition supplied here |
| --- | --- |
| Original impaired-backing selector | The backing provider now also holds and exercises the actual oracle role; the cold admin replaces that oracle without its signature. Both assets and both sides cross replacement before/after impairment. |
| Lane 24 resolved containment | This product has two live open legs, owner-local liens and exact-expiry impairment, with no resolution or admin burn. |
| Lane 14 open insurance rotation | The protected subject is encumbered backing, eventually impaired-only, rather than funded insurance. No Hybrid feed or oracle round trip is used. |
| Older cold-admin ABA / zero-role tests | Neither cold-admin ABA nor zeroing is used. No retained signed management envelope is claimed. |
| Earned-reserve / Lane 11 consumed-backing tests | Subject earnings and consumed backing remain zero. Its nonzero impaired lien independently keeps the funded role locked. The peer's consumed reserve arises from ordinary PnL netting and is separately accounted. |

## Public trace and independent checks

Eight worlds cross asset 0/1, positive/negative winning exposure, and the two
orders of oracle replacement and slot-3 expiry. System/SPL/ATA/wrapper
instructions construct all economic state. Only SBF loading, SOL airdrops and
Clock warps use harness setup; initialized program-owned economic bytes are never
injected, replaced or restored by the test.

1. Public deposits give the winner 313, counterparty 5,000 and bystander 20 atoms.
   The provider contributes 150 backing atoms; SPL mint authority is revoked at
   5,483 atoms. Bilateral trades open 20 units on the winning asset and 10 on the
   adverse asset at 100, then increase the adverse exposure by two units after
   marks move to 105/95 or 95/105. Public cranks create a nonzero source-backed
   owner-local lien. The provider is installed as oracle and publishes a report.
   A distinct cold admin, incoming oracle and market authority have zero receipts.
2. In one schedule the incumbent reports the expiry mark and public cranks impair
   the lien before replacement. In the other the incoming oracle reports that
   mark after replacement. Expiry moves the winning mark one additional unit;
   the adverse mark stays fixed. The new selector completes leg refresh with at
   most four additional public cranks per trader, stopping as soon as the
   independent certificate check confirms that portfolio is current.
3. A five-atom bystander payout, cold-signed oracle replacement and incoming
   report form the successful prefix of two rejected bundles. A current-epoch
   backing-role seizure rejects `EngineLockActive`; a one-atom withdrawal to the
   incoming oracle's own valid ATA rejects `Unauthorized`. Both reject at
   instruction 5, after three successful wrapper instructions and one successful
   SPL transfer. Complete tracked and compiled Accounts roll back, except the
   separate payer's exact signature fee.
4. Reusing the three instructions commits without provider or market-authority
   signatures. Full economics change only by the five-atom owner payout; only the
   subject oracle key, authority epoch and observation sequence change in the
   management state. Reports use authenticated Clock slots despite `u64::MAX`
   payload hints. The peer profile and control sequences remain exact.
5. A current-epoch report from the former oracle rejects `Unauthorized`, restoring
   another executed owner SPL payout. After both schedules reach impairment,
   cold-admin seizure again rejects `EngineLockActive` after a real SPL prefix.
   All other funded bucket terms on the subject, including its sibling side,
   are zero; owner-local impaired backing is positive and equals source/bucket
   totals. Thus the rejection cannot be explained by fresh backing or earnings.
6. An owner-signed two-unit adverse-leg reduction succeeds with the impaired lien
   still positive. The original provider can then consent to transfer backing
   to the incoming oracle, changing only that role and one authority epoch, with
   full economics and the peer profile/sequences unchanged. The bystander receives
   its remaining 15 atoms without any privileged signer.

The independent book derives equity from input sizes and prices: 363/4,950 before
expiry, then 383/4,930 after expiry. Positions remain exactly 20/12 units until
the explicit reduction returns them to 20/10. The 50-atom peer loss reserve
becomes 30 fresh plus 20 consumed atoms when the counterparty offsets its new
20-atom loss against its existing peer claim. Subject fresh plus valid backing
before expiry equals `(150 + 100) * BOUND_SCALE`; after expiry only the positive
impaired lien remains in its funded-role predicate.

Every suffix attempt checks portfolio ownership, local liens, independent stock
and encumbrance censuses, fixed supply, engine/SPL custody and exact wallet
amounts. Successful SPL changes may alter only token amounts; program Account
owners, lamports, lengths and other metadata remain fixed. Rejections also check
packet size, cryptographic signatures, exact error/index and successful prefix
counts through the existing transaction helper. The two schedules finish with
identical source-credit state, backing buckets, custody, capital totals and
per-trader capital/PnL, without comparing unrelated generated keys.

## Validation

Logs: `/dev/shm/row416-impaired-20260916-logs/`.
Wrapper SBF SHA-256:
`e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f`.
Existing build caches were copied into private target directories; both wrapper
and host suites were compiled from this clone. No matcher fixture is required.

```sh
git clone --no-local --single-branch --branch codex/astra-invariant-cycle-20260915 /tmp/percolator-astra-invariant-cycle-20260915-run /dev/shm/percolator-row416-funded-succession-20260916
cd /dev/shm/percolator-row416-funded-succession-20260916
git switch -c codex/row416-funded-succession-20260916
cp -a /dev/shm/lane24-20260916-host /dev/shm/row416-impaired-20260916-host
cp -a /dev/shm/lane24-20260916-sbf /dev/shm/row416-impaired-20260916-sbf
mkdir -p /dev/shm/row416-impaired-20260916-logs /dev/shm/row416-impaired-20260916-tmp
env CARGO_TARGET_DIR=/dev/shm/row416-impaired-20260916-sbf CARGO_BUILD_JOBS=4 TMPDIR=/dev/shm/row416-impaired-20260916-tmp RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/row416-impaired-20260916-sbf/deploy -- --locked

export CARGO_TARGET_DIR=/dev/shm/row416-impaired-20260916-host
export CARGO_BUILD_JOBS=6 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export TMPDIR=/dev/shm/row416-impaired-20260916-tmp
export PERCOLATOR_FUZZ_SBF=/dev/shm/row416-impaired-20260916-sbf/deploy/percolator_prog.so
cargo test --locked --offline --test v16_cu --test v16_program_fuzz_regressions --no-run
cargo test --locked --offline --test v16_cu inv_005_authority_incarnation_binding::impaired_backing_containment::cold_oracle_impaired_containment::v16_program_cold_oracle_replacement_commutes_with_open_lien_impairment -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_005_authority_incarnation_binding::impaired_backing_containment::v16_program_cold_admin_cannot_seize_impaired_backing_only_role \
  inv_005_authority_incarnation_binding::funded_oracle_succession::cold_oracle_funded_containment::v16_program_cold_oracle_replacement_preserves_funded_coholder_and_atomic_prefix \
  inv_005_authority_incarnation_binding::funded_oracle_succession::cold_oracle_insurance_containment::v16_program_funded_insurance_coholders_preserve_open_positions_through_oracle_round_trip \
  inv_005_authority_incarnation_binding::funded_oracle_succession::cold_oracle_resolved_containment::v16_program_cold_oracle_resolution_and_admin_burn_preserve_split_funded_recipients \
  inv_005_authority_incarnation_binding::v16_program_funded_role_guard_and_oracle_handoff_are_source_complete
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_dated_open_security_finding_benchmark_is_non_overclaiming \
  inv_079_public_reachability_evidence::v16_public_instruction_coverage_registry_matches_production_roster \
  inv_079_public_reachability_evidence::v16_program_invariant_harnesses_are_test_free_roots \
  inv_079_public_reachability_evidence::v16_special_verification_method_registry_matches_charter \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row \
  inv_079_public_reachability_evidence::v16_every_public_trace_consumer_validates_reachability_evidence
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_005_impaired_backing_containment.rs tests/invariants/cu/inv_005_cold_oracle_impaired_containment.rs
git diff --check
git diff --cached --check
git diff --exit-code 63cc6f284674f31187430f52d43a201b21b95d50 -- src Cargo.toml Cargo.lock tests/fixtures ':(glob)**/*.tsv'
git show --format= --check HEAD
sha256sum "$PERCOLATOR_FUZZ_SBF"
```

| Check | Result |
| --- | --- |
| Rebuilt wrapper SBF and both host test binaries | PASS |
| Final new selector | PASS, 1/1, eight worlds, 4.96 seconds; `new-final.log` |
| Five adjacent controls | PASS, 5/5, 18.40 seconds; `controls.log` |
| Eight INV-079 guards | PASS, 8/8, 0.71 seconds; `inv079.log` |
| Touched Rust formatting, Git whitespace, protected-file diff | PASS |

The final runtime pass checked eight worlds, 32 exact SPL-prefix
rollbacks, eight cold oracle replacements, eight strict reductions, eight
incumbent-consented funded handoffs and 16 committed owner payouts. Measured
CU peaks were 466,278 for selected public setup/expiry transactions, 345,471 for
rejected bundles and 284,726 for committed suffixes. Final enforced ceilings are
750,000 / 600,000 / 345,000 respectively, with the existing 300,000 ceiling on
single-instruction management/custody calls. These peaks exclude unmeasured
initial account creation/funding instructions and rejected no-op crank probes.
Control peaks: original impaired role `[465879, 142569, 344457]`, flat funded
oracle 60,503, Lane 14 open insurance 136,165 and Lane 24 resolved containment
52,969 CU. The source-composition control does not execute SBF.

Development runs exposed test-model omissions for peer loss-created backing and
its fresh-to-consumed netting, followed by an incomplete asset-1 refresh and an
overlong refresh schedule entering legitimate partial liquidation. Those were
corrected in the test: expected amounts now derive from trade inputs, and refresh
stops at an independent current certificate. No production failing witness or
red/green production fix is claimed. Intermediate logs are retained as
`new-initial.log`, `new-stock-model.log`, `new-peer-netting.log`,
`new-full-model.log`, `new-refreshed.log`, and `new-certified.log`.

## Remaining status

No public-route LoF, DoS or CU bug was found. This supplies bounded independent
containment evidence for actual oracle/backing coholders with open impaired
liens, not generic closure of funded-oracle takeover.

Row 416 remains **OPEN/missing** and INV-005 remains **REFUTED_CURRENT**.
All related invariant and finding dispositions are unchanged. Arbitrary
management/price histories, adversarial oracle moves, multiple claim owners,
beneficiary succession through resolution, retained signed envelopes, Hybrid,
nonzero fees/funding, insurance coholders, optional ledgers, alternate quote
rails, maximum shapes, full provider/trader exits and terminal closure remain
outside this finite product. The test intentionally ends with live exposure and
still-attributed impaired backing.
