# Row 416: Burned Asset-Admin Restoration

Four bounded public LiteSVM histories pass, with 104 exact Account/SPL rollback
checks and peak measured CU 65,434. No current behavior violation was found.
**Row 416 remains OPEN/missing; INV-005 remains `REFUTED_CURRENT`.** No machine
disposition is promoted.

## Reachable Disable/Restore Schedule

The public wrapper rejects zero oracle, backing, insurance-beneficiary and
insurance-operator keys. Only the cold asset-admin role can be burned. Its
zero-to-A restoration is unauthorized within the same generation; even the
market authority does not have an `UpdateAssetAuthority` override for a burned
admin. This test therefore exercises actual cold-admin disable/restore across
public asset retirement and reactivation. It does not synthesize a disabled
oracle or claim a same-generation restoration history.

The product is `{A -> zero, A -> B -> zero}` x `{AuthMark, EWMA}` on asset 1.
A initially coholds the cold-admin, oracle, backing-provider and both insurance
roles. The market authority, B and the asset-0 peer provider are distinct.
Public SPL minting supplies A with 100 atoms and the peer with 23, then removes
mint authority. A deposits 41 backing and 59 insurance atoms; the peer deposits
23 backing atoms. No positions, liabilities, earnings or impairment exist.

1. Retain three signed variants of each of six requests: mark report, oracle
   configuration, admin handoff, oracle handoff, backing exit and insurance
   exit. Each starts with a peer backing payout of 1, 2 or 3 atoms. All eighteen
   envelopes and a separate complete peer exit simulate successfully before
   succession, without changing their complete account frames.
2. Optionally transfer the cold-admin role to B. Each required-role zeroing
   attempt rejects `InvalidInstruction` after the peer SPL prefix. Burn the
   cold-admin role through its incumbent. Only its profile field and the
   subject's authority epoch change; both funded claims remain intact.
3. A cannot self-restore the burned admin with a current epoch. The original
   signed oracle, report, configuration and payout requests reject
   `EngineStale`; the old admin handoff rejects `Unauthorized`. B also cannot
   withdraw either funded claim with a current epoch and A's valid destination.
   Each failed suffix restores the completed peer payout exactly.
4. A commits its independently authorized 41-atom backing and 59-atom insurance
   exits while the admin remains zero. The market authority retires the empty
   asset. Advance the authenticated clock from slot 2 to slot 3 to satisfy the
   retirement cooldown. A bundle containing a peer payout, public reactivation
   and the old report rejects `AssetGenerationMismatch` at index 4. Full
   rollback includes the lifecycle, new generation allocation and SPL transfer.
5. Commit reactivation. Its bootstrap admin consents to restoring A. Public
   oracle self-handoffs recreate the original authority epoch; reconfiguration
   and top-ups from A's withdrawn atoms recreate every original control counter.
   The whole restored profile equals its original image except the two oracle
   clock fields, which move from 2 to 3. The asset generation is strictly newer.
6. Six current-generation controls simulate successfully with the same signer,
   epoch, observation number, amount, report payload and prefix as their old
   counterparts. Only the asset-generation binding in the request changes.
   Submit the originally signed 1-atom-prefix envelopes unchanged: all six
   reject `AssetGenerationMismatch` after the peer SPL payout in Live state.
7. Resolve publicly and submit the originally signed 2-atom-prefix variants.
   All six reject at the same generation check with full terminal-frame
   rollback. The untouched asset-0 peer's original complete exit commits.
   A commits fresh backing and insurance exits. A ends with exactly 100 atoms,
   the peer with 23, B with zero, and the vault and attributed stocks with zero.

All retained signed bytes are checked against their pre-succession serialized
images. Restored replays verify their signatures and still-current blockhash;
there is no signature-check bypass, blockhash refresh or re-signing of retained
envelopes. The atomic reactivation probe deliberately combines an old instruction
with a newly signed current prefix, and is not counted as an immutable envelope.

The shared `land` helper snapshots all compiled account keys plus economic
sentinels, checks exact errors and successful wrapper/SPL prefix counts, and
compares complete `Account` images including owner, lamports, data, executable
flag, rent epoch and absent accounts. Only the payer's exact default signature
fee may change on rejection. An independent book checks all backing and
insurance domain stocks, zero encumbrance/liabilities, exact token Account
images, fixed mint supply and unchanged asset-0 profile/control sequences.

Setup uses public System, ATA, SPL and Percolator instructions plus LiteSVM
airdrops, program loading and authenticated clock advancement. No program-owned
byte mutation or injected oracle account is used. No matcher artifact is needed.

## Limits and Non-Overlap

This increment covers the burnable asset-admin role, restored through a new
asset generation with an intentionally repeated authority epoch. It adds that
composition to existing cold-admin burn, oracle ABA and asset-reuse checks.
It does not duplicate the recent open-lien impairment or retained terminal
oracle/role closure products: there are no liens, positions, ledger accounts,
external reports, slab/vault closure, reserve dust or rent reclamation here.

Coverage is finite: classic SPL, two configured assets, one reused non-base
asset, two admin schedules, two managed-mark modes, flat funded claims and one
recent-blockhash window. Required-role zeroing is rejected, not a committed
transition. A's subject claims must exit before retirement and are then
re-funded from the same withdrawn atoms; the peer stays funded throughout.
Same-generation burned-admin restoration, generic scheduling, native/secondary
quote, external oracle feeds, receipt liabilities, ledger reuse and max-shape
execution are not established. Fresh oracle/role controls are simulations,
whereas restored activation, funding and final payouts commit.

## Isolation and Artifacts

- Source repository: `/tmp/percolator-astra-invariant-cycle-20260915-run`.
- Base ref at worktree creation: `origin/codex/astra-invariant-cycle-20260915`,
  `4d17c967bdc09ad02f4ea0d68693e6998a9d14b9`.
- Local branch: `codex/row416-disabled-oracle-restoration-20260916-sidecar`.
- Worktree: `/dev/shm/row416-disabled-oracle-restoration-20260916-sidecar`.
- Private host target and temporary/log directory:
  `/dev/shm/row416-disabled-oracle-restoration-20260916-{target,tmp}`.
  The host cache was copied, not shared, from the existing terminal worker.
- Reused SBF: `/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so`.
- SBF SHA-256:
  `e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f`.
- Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
- Protected baseline: `origin/main` at
  `d809e9a563d9b8bf38f32648b32a15d75f526ec8`.

The only changes are the new Rust child, its three-line mount in
`cu/inv_005_cold_admin_handoff_scope.rs`, the README entry and this report.
`/home/anatoly/percolator-prog` is not edited. No production, Cargo, fixture or
invariant TSV changes are included. All work remains local.

## Validation

From the isolated worktree:

```sh
export CARGO_TARGET_DIR=/dev/shm/row416-disabled-oracle-restoration-20260916-target
export TMPDIR=/dev/shm/row416-disabled-oracle-restoration-20260916-tmp
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so

cargo test --locked --offline --test v16_cu inv_005_authority_incarnation_binding::cold_admin_handoff_scope::disabled_role_restoration::v16_program_burned_admin_restoration_keeps_signed_oracle_and_funded_requests_stale -- --exact --nocapture --test-threads=1

cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_005_authority_incarnation_binding::v16_program_oracle_authority_aba_is_asset_scoped_and_rolls_back_retained_prefix \
  inv_005_authority_incarnation_binding::cold_admin_handoff_scope::terminal_oracle_role::v16_program_retained_terminal_envelopes_bind_funded_oracle_roles_and_slab_closure

cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_program_invariant_harnesses_are_test_free_roots

rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_005_cold_admin_handoff_scope.rs tests/invariants/cu/inv_005_disabled_role_restoration.rs
git diff --check
git show --format= --check HEAD
git diff --exit-code origin/main -- src Cargo.toml Cargo.lock tests/fixtures ':(glob)tests/invariants/*.tsv'
```

The new selector passes 1/1: four histories, 104 rollbacks, peak 65,434 CU under
the asserted 300,000 bound. This peak covers the measured history steps and
prevalidations, not common market/mint setup or top-ups; it is not a worst-case
proof. Keypair/PDA variation can change the measured peak. Logs are `new.log`,
`controls.log` and `metadata.log` in the private temporary directory.

Both adjacent selectors pass (2/2); the existing terminal-envelope control
measures 119,276 CU. All three metadata gates pass (3/3). Rustfmt, whitespace
checks and the protected diff against `origin/main` are clean. The broader
INV-005 suite is not claimed green or rerun by this worker.

Development corrections addressed a mutable simulation borrow, an invalid
destination in B's insurance probe, and an attempted same-slot reactivation
before the required cooldown. None was acceptance of stale authority or a
rollback failure; the stale-request property was not relaxed.
