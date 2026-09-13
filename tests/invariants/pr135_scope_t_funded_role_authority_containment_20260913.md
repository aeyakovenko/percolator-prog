# PR135 Scope T: Funded Role Authority Containment

Base: `d134c64d788e49264ecc0187a75209053925f43a`, fetched from
`origin/codex/astra-open-holdout-ledger-20260912` on 2026-09-13 UTC.
Branch: `codex/pr135-scope-t-funded-role-authority-containment-20260913`.
Active worktree: `/tmp/percolator-scope-t-20260913/target/worktree`.

Row 416 is a coverage label. Its OPEN status, the open-finding evidence, and
machine invariant classifications remain unchanged. This is one new bounded
public LiteSVM conformance family owned by INV-005, with related INV-020,
INV-024, INV-027, INV-055 and INV-081 checks. No implementation change.

## Existing Coverage

Read the repository README, invariant charter, invariant README, open findings
and row 416, then the existing INV-005 families on the requested base.
No external PR branch, diff or test was inspected.

| Existing family | Already covered |
| --- | --- |
| `inv_005_authority_incarnation_binding.rs` | Source-complete role/epoch matrices; ordinary scoped authority gates; unfunded oracle ABA; funded insurance succession preserving separate oracle/operator roles; backing withdrawal ABA. |
| `inv_005_funded_oracle_succession.rs` and `inv_005_cold_oracle_funded_containment.rs` | Oracle succession after admin burn; cold-admin oracle replacement over a backing coholder; exact observation and SPL-prefix rollback when a funded-role suffix fails. |
| `inv_005_funded_backing_succession.rs` | Paid principal prefixes, unpaid backing succession and Live/terminal role partition. |
| `inv_005_retained_insurance_management.rs` | Empty-to-funded management revalidation and the two orders of separately held insurance policy/operator consent. |
| `inv_005_cold_admin_handoff_scope.rs` and `inv_005_funded_role_zero_transition.rs` | Cold-admin ABA/burn across funded roles; Active/DrainOnly scope; atomic rejection of disabled-role suffixes after handoff and payout. |
| Earned-reserve, consumed-backing and backing-refunding modules | Partial principal repayment with earned fees; consumed receivables through expiry; depletion, empty succession and refunding. |
| Funded-insurer stale-resolution and shutdown-reserve ABA modules | Beneficiary succession across authenticated stale resolution; shutdown backing/insurance reserves and terminal role partition. |

Those families do not exhaust interleavings of three independent funded-role
round trips while executing observations and partial payouts after every handoff.
The public funding fixture and complete-Account transaction checker are reused;
the word generator, history ledger, interleaved consumers and coverage relation
are new. The fixture is reused by mounting this module below the zero-transition
module; its existing test is not invoked or changed.

## Generated Coverage

Owner: [cu/inv_005_generated_funded_role_epochs.rs](cu/inv_005_generated_funded_role_epochs.rs).

```text
inv_005_authority_incarnation_binding::cold_admin_handoff_scope::funded_role_zero_transition::generated_funded_role_epochs::v16_program_generated_funded_role_round_trips_preserve_entitlements_and_observation_scope
```

- Exhaust all 90 length-six words containing each of three roles twice.
  Each role follows A -> B -> A; outward and return transitions may interleave.
- Cross both target assets with two role shapes: oracle/backing/operator, with
  the oracle initially coheld by the backer; oracle/backing/insurance policy,
  with the oracle initially coheld by the insurer. Total: 360 worlds.
- Insert a separately signed cold-admin A -> B -> A pair at word_index modulo
  seven. Both backing and insurance payout orders occur, chosen by word parity.
- Seed `XorShiftRng` with `0x005_2026_0913` for six 1..=3-atom partial payout
  amounts per word. Backing alternates between the target's long/short domains.
- Publicly fund backing `[31,43,59,71]`, insurance `[17,23,29,37]`, and a
  101-atom senior portfolio. Freeze the 411-atom SPL supply. Each managed role
  remains funded throughout its word; the sibling asset stays funded and scoped.
- After each handoff, a different current funded-role holder submits a current
  epoch/sequence observation and must be rejected as outside oracle scope.
  The configured oracle then successfully submits the same-price observation.
- After each observation, pay the generated backing and insurance slices to
  their independently tracked current recipients. A policy transfer leaves
  the hot operator's payout entitlement unchanged. Returning A receives only
  the unpaid balance, while earlier consented B payouts stay attributed to B.

## Oracles

The stock/recipient book starts from funding inputs and committed actions.
It accounts for the shared per-asset authority epoch: eight role/admin changes
and six live insurance debits advance the target by fourteen before final exits.
It never infers the expected recipient or payout quantity from a returned balance.
At each checked transaction it reconciles exact domain principal and insurance,
wallet amounts, vault custody, frozen supply, senior capital, zero PnL/source
claims/liens/earnings/OI, Live/Active admission, all profiles and control sequences.
Management additionally preserves the entire decoded market economy.

Every successful current observation is signed by the current oracle holder.
Despite `now_slot = u64::MAX`, last-good time follows authenticated slots 2..7.
Other role changes preserve the oracle profile, and target changes preserve
the sibling's full asset state and profile/sequence bindings.

Three handoff transactions are signed before the word, each preceded by a
one-atom sibling backing payout. After every original holder returns, all three
must fail specifically at the handoff with EngineStale. Handoffs have no
observation or stock watermark that could mask the authority-epoch check.
A separately signed current-sequence observation with the initial epoch also
fails EngineStale; changing only its epoch succeeds. This observation pair is
an epoch-isolation control, not a claim that its signature was retained.

Two correctly signed current cold-admin replacements of funded economic roles
fail EngineLockActive. All twelve rejected transactions per world execute the
sibling SPL payout first and restore complete compiled/tracked Accounts,
including account absence, metadata, lamports and economic sentinels.
The exact payer signature fee is the only permitted rollback delta.
The original sibling payout transaction then succeeds unchanged.

Finally, current role holders receive all remaining domain backing and insurance,
and the user withdraws all 101 senior atoms. Custody reaches zero. The insurer
and cold-admin wallets remain zero; B receives a nonzero consented payout.
Every transaction verifies signatures, packet size <= 1232 and CU <= 600000.

## Limits

This is a finite Live/Active, flat-portfolio, same-price AuthMark family.
It does not certify price manipulation containment, trading health certificates,
Hybrid/EWMA/external feeds, outstanding exposure, pending liabilities, earned
reserves, consumed backing, Recovery/Resolved retirement, expiry, native quote
custody, optional telemetry, policy economics or arbitrary histories.
INV-027 evidence is preservation and full exit of flat senior principal;
INV-055 evidence is the sampled Live/Active route admission; INV-081 evidence
is the explicit successful-route postconditions above, not a global theorem.

Required domain roles cannot be disabled on this base. Existing zero-role/burn
families cover that boundary; this increment uses A -> B -> A only.
The two coholder shapes are coupled to the insurance role choice. Payout order,
admin insertion point and generated slices are sampled per word, not a Cartesian
product of all possibilities. Initial reserve quantities are fixed.
Observation authority has no independent cash reserve: its holder coholds a
funded economic role. Consented role succession intentionally transfers its
unpaid role entitlement. No claim is made that cold-admin oracle replacement
itself is forbidden on this base.

## Validation

Final exact probe: PASS 1/1 in 191.67s, 360 worlds, 16920 checked transactions,
4320 exact rollback/SPL prefixes, peak 56882 CU. The exact selector's `--list`
check finds one test. All four metadata gates PASS (4/4). Repository formatting,
working/staged diff checks and the post-commit show check PASS. Production, Cargo
inputs, open findings and machine invariant status files compare unchanged.
No full-suite, Kani or arbitrary-history proof is claimed.

Adjacent controls: 8 PASS, 4 FAIL. All four failures also reproduce on an
untouched detached worktree of `d134c64d` at `target/base-control`, using the
same SBF artifact. They assert an unchanged epoch after a live insurance debit;
the base now consumes that epoch. Existing control assertions are unchanged:

- `inv_005_authority_incarnation_binding::retained_insurance_management::v16_program_retained_empty_insurance_management_rechecks_stock_before_ordered_succession`
- `inv_005_authority_incarnation_binding::cold_admin_handoff_scope::v16_program_cold_admin_aba_and_burn_preserve_funded_handoff_scope_and_value`
- `inv_005_authority_incarnation_binding::cold_admin_handoff_scope::funded_role_zero_transition::v16_program_zero_role_suffix_restores_funded_handoff_payout_and_retained_consent`
- `inv_005_authority_incarnation_binding::v16_program_funded_insurance_handoff_preserves_incumbent_oracle_and_operator`

Logs are under `/tmp/percolator-scope-t-20260913/target/`:
`scope-t-probe.log`, `scope-t-controls.log`, `scope-t-base-controls.log`,
`scope-t-metadata.log`.
The final run uses a separate host target directory. A shared-target invocation
after the baseline build selected zero tests and is explicitly not evidence.

SBF artifact SHA-256:
`49f49d1a8a7a3c20e4e3e09c635c4d5ce08925056030668b6838ccf7224800b0`.
The artifact was built from this exact base, with default Anchor v2 features
and platform-tools v1.52; the production source and Cargo inputs are unchanged.

## Commands

The original worktree was created at `/tmp/percolator-scope-t-20260913`.
Shared root-filesystem exhaustion interrupted a test-file write. A 6-GiB tmpfs
under that worktree's `target` now holds the active worktree and its private
Git metadata (`target/scope-t.git`). The private bare clone shares read-only
base objects with the original repository and contains only this task branch.
The main checkout and `/tmp/percolator-astra-watch.Cb2E7d` were not edited.

```sh
git fetch origin refs/heads/codex/astra-open-holdout-ledger-20260912:refs/remotes/origin/codex/astra-open-holdout-ledger-20260912
git worktree add -b codex/pr135-scope-t-funded-role-authority-containment-20260913 /tmp/percolator-scope-t-20260913 origin/codex/astra-open-holdout-ledger-20260912
# SBF build, in the original isolated worktree with its private target mount:
env CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir target/deploy -- --locked
# Final active worktree and isolated host target:
cd /tmp/percolator-scope-t-20260913/target/worktree
export CARGO_TARGET_DIR=/tmp/percolator-scope-t-20260913/target/scope-t-host
export PERCOLATOR_FUZZ_SBF=/tmp/percolator-scope-t-20260913/target/deploy/percolator_prog.so
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
cargo test --locked --offline --test v16_cu inv_005_authority_incarnation_binding::cold_admin_handoff_scope::funded_role_zero_transition::generated_funded_role_epochs::v16_program_generated_funded_role_round_trips_preserve_entitlements_and_observation_scope -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_005_authority_incarnation_binding::cold_admin_handoff_scope::funded_role_zero_transition::generated_funded_role_epochs::v16_program_generated_funded_role_round_trips_preserve_entitlements_and_observation_scope -- --exact --list
# Adjacent controls (initial run used the original target before baseline compilation):
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_005_authority_incarnation_binding::funded_oracle_succession::v16_program_funded_oracle_succession_after_admin_burn_preserves_backing_exit \
  inv_005_authority_incarnation_binding::funded_oracle_succession::cold_oracle_funded_containment::v16_program_cold_oracle_replacement_preserves_funded_coholder_and_atomic_prefix \
  inv_005_authority_incarnation_binding::funded_backing_succession::v16_program_funded_backing_succession_preserves_paid_prefix_and_terminal_role_partition \
  inv_005_authority_incarnation_binding::retained_insurance_management::v16_program_retained_empty_insurance_management_rechecks_stock_before_ordered_succession \
  inv_005_authority_incarnation_binding::cold_admin_handoff_scope::v16_program_cold_admin_aba_and_burn_preserve_funded_handoff_scope_and_value \
  inv_005_authority_incarnation_binding::cold_admin_handoff_scope::funded_role_zero_transition::v16_program_zero_role_suffix_restores_funded_handoff_payout_and_retained_consent \
  inv_005_authority_incarnation_binding::cold_admin_earned_reserve::v16_program_cold_admin_rotation_preserves_earned_reserve_after_partial_principal_repayment \
  inv_005_authority_incarnation_binding::v16_program_funded_insurance_handoff_preserves_incumbent_oracle_and_operator \
  inv_005_authority_incarnation_binding::v16_program_oracle_authority_aba_is_asset_scoped_and_rolls_back_retained_prefix \
  inv_005_authority_incarnation_binding::v16_program_backing_withdrawal_aba_rolls_back_spl_and_ledger_prefix \
  inv_005_authority_incarnation_binding::v16_program_adversarial_role_containment_matrix_is_source_complete \
  inv_005_authority_incarnation_binding::v16_program_authority_epoch_matrix_is_source_complete
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row \
  inv_079_public_reachability_evidence::v16_post_pr135_counterexamples_reopen_every_affected_invariant
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git show --format= --check HEAD
git diff --exit-code d134c64d -- src Cargo.toml Cargo.lock tests/invariants/open_findings.tsv tests/invariants/invariant_status.tsv
```

Baseline confirmation uses `target/base-control`, the original `target` as
`CARGO_TARGET_DIR`, the same build settings, and the four exact failing selectors
listed above. No implementation mismatch with the sampled invariant was observed;
the adjacent failures are pre-existing test expectations, not new behavior.
