# Row 416: Retained Terminal Oracle and Role Envelopes

Four public LiteSVM histories pass, including 30 exact Account rollbacks and
four complete terminal closures. No current behavior violation was found.
**Row 416 remains OPEN/missing; INV-005 remains `REFUTED_CURRENT`.** This finite
increment does not promote the machine status or establish generic closure.

## Isolation and Artifacts

- Source repository: `/tmp/percolator-astra-invariant-cycle-20260915-run`.
- Base: `origin/codex/astra-invariant-cycle-20260915` at worktree creation,
  `b4091247021656d6877e2454ed04e4b0f8cbfefa`.
- Isolated worktree: `/dev/shm/row416-terminal-oracle-role-20260916`.
- Local branch: `codex/row416-terminal-oracle-role-20260916`. No push.
- `/home/anatoly/percolator-prog` was not edited.
- Reused SBF: `/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so`.
- SBF SHA-256:
  `e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f`.
- Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
- Private host target, temporary files and logs use
  `/dev/shm/row416-terminal-20260916-{target,tmp,logs}`. No matcher fixture or
  fixture-local symlink is needed.
- Protected files match `origin/main` at
  `d809e9a563d9b8bf38f32648b32a15d75f526ec8`.

The changes are the new [Rust child](cu/inv_005_terminal_oracle_role.rs), its
three-line mount in [cold-admin scope](cu/inv_005_cold_admin_handoff_scope.rs),
the [README](README.md) entry and this report. No production source, Cargo file,
fixture or invariant TSV changes are included.

## Public History and Oracles

The product is `{asset 0, asset 1}` x `{oracle, insurance beneficiary}`. A is
the oracle, backing provider, insurance beneficiary and insurance operator.
The cold admin, market authority, peer provider and prospective successors are
distinct. A deposits 41 backing and 59 insurance atoms; the peer deposits 23
backing atoms on the other asset. Seven additional atoms are minted directly to
the canonical vault as unbooked surplus, then mint authority is permanently
removed. Ledger accounts are System-created and initialized by public top-ups.
The market resolves through its public signed instruction before retention.

1. Before either rotation, retain signed envelopes for the peer payout, the
   peer-payout/role-handoff pair, all three reserve payouts plus slab closure,
   and standalone slab closure. Simulate the first three successfully without
   changing any tracked account. The standalone close contains the same close
   bytes as the prevalidated bundle tail; it is not independently admissible
   until the reserves have been paid.
2. Rotate the subject role A-to-B-to-A. For oracle succession, the cold admin
   signs with each incoming holder; the outgoing oracle does not sign. For
   beneficiary succession, incumbent and incoming holder consent. After the
   first replacement, A's current-epoch role handoff rejects `Unauthorized`
   after the peer's completed SPL payout, while A's independent backing exit
   is still admissible. Full rollback restores the peer payout.
3. After restoration, submit the original signed payout and handoff envelopes.
   Both reject `EngineStale` after the peer's SPL prefix. Verify signatures,
   unchanged retained message bytes and the same still-current blockhash.
   The peer's original envelope, a renewed handoff and a renewed complete
   terminal bundle remain admissible.
4. Replace only the payout epochs in the original terminal bundle. Its old
   close suffix rejects after all three payouts when asset 0 rotated. When
   asset 1 rotated, the old close epoch is still admissible: that role change
   did not change market-authority scope. These are simulation controls or
   exact rollback probes, so the reserves remain funded.
5. Rotate the separate market authority to C and back. Asset profiles and all
   funded claims remain unchanged; only asset 0's epoch advances twice.
   Submit the retained standalone close: `EngineStale`. The previously valid
   signed terminal bundle also rejects at asset 0's payout. Fresh payout
   instructions followed by its old close suffix execute three transfers,
   then reject `EngineStale` with exact rollback in every world.
6. Pre-sign and prevalidate the final current bundle. Append malformed System
   data to an otherwise identical bundle: all four wrapper instructions and
   five SPL calls complete before `InvalidInstructionData` at index 6.
   The entire vault closure, 7-atom sweep, slab tombstone, rent reclamation,
   reserve payouts and ledger changes roll back. The retained successful
   bundle then commits unchanged.

The independent reserve book requires 100 atoms for A, 23 for the peer and
zero for B/C. Only the 7 donated atoms go to the market authority. It checks
domain stocks, zero consumed/impaired/liened stock, zero liabilities, both
profiles, full control-sequence structs, ledger attribution and fixed SPL
supply. Backing payouts preserve epochs; insurance payout consumes one epoch.
Final token accounts equal their original empty Account images with only the
expected amount changed. Vault closure and slab shrinkage refund exact rent;
the market retains the rent-exempt tombstone and the mint image stays fixed.

Every rejected transaction compares complete Accounts for all compiled keys
and economic sentinels, including absent accounts, owners, lamports, data,
executable flags and rent epochs. The payer differs only by the exact default
signature fee. Success-log counts establish that the asserted rollback
prefixes actually ran. Setup uses public System, ATA, SPL and Percolator
instructions plus LiteSVM airdrops/program loading. No account/state injection,
private engine mutation, disabled signature checks or refreshed retained
envelopes are used.

## Non-Overlap and Limits

| Existing coverage | New terminal product |
| --- | --- |
| Lane 14 funded AuthMark/Hybrid round trip | No positions, feed movement or Hybrid accrual. This exercises signed reserve and close envelopes through actual vault/slab deletion. |
| Lane 24 resolution/cold burn | Adds role ABA, immutable signed envelopes, independent-asset close scoping, market-authority ABA and full terminal closure rollback. No cold-admin burn is claimed here. |
| Lane 28 open-lien impairment | No liens, impairment or expiry. Fresh principal and insurance remain funded across terminal succession. |
| Existing shutdown reserve ABA | Market is Resolved throughout retention and succession. Includes retained signed transactions, market-authority return and final slab closure instead of market-Live asset shutdown. |
| Cold-admin scope and generated funded role epochs | Carries the retained signatures through terminal payouts, vault deletion and rent reclamation, not only live role management. |
| Legacy stale-market-authority close test | Uses public funded setup, A-to-B-to-A, prevalidated signed envelopes and successful-prefix rollback; no injected vault amount. |
| Insurance successor/expiry residue closure | Tests authority incarnation and signed envelope validity, not source expiry, custody recreation or insurance recredit. |

Coverage is bounded to classic SPL, two configured assets, two role kinds,
one round trip per role and market authority, no positions, no debt/receipts,
no source expiry and one recent-blockhash window. It does not cover native or
secondary quote, Hybrid or external oracle reports, disable/restore, asset
generation reuse, max-shape scans, adversarial scheduling or arbitrary authority
histories. Required oracle/funded roles are not disabled in this product.
Simulation controls establish admissibility, not an additional committed payout.

## Validation

Run from the isolated worktree with:

```sh
export CARGO_TARGET_DIR=/dev/shm/row416-terminal-20260916-target
export TMPDIR=/dev/shm/row416-terminal-20260916-tmp
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so

cargo test --locked --offline --test v16_cu inv_005_authority_incarnation_binding::cold_admin_handoff_scope::terminal_oracle_role::v16_program_retained_terminal_envelopes_bind_funded_oracle_roles_and_slab_closure -- --exact --nocapture --test-threads=1

cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_005_authority_incarnation_binding::cold_admin_handoff_scope::v16_program_cold_admin_aba_and_burn_preserve_funded_handoff_scope_and_value \
  inv_005_authority_incarnation_binding::funded_oracle_succession::cold_oracle_resolved_containment::v16_program_cold_oracle_resolution_and_admin_burn_preserve_split_funded_recipients \
  inv_005_authority_incarnation_binding::funded_oracle_succession::shutdown_reserve_aba::v16_program_shutdown_and_funded_oracle_aba_preserve_separate_reserve_owners

cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_program_invariant_harnesses_are_test_free_roots

rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_005_cold_admin_handoff_scope.rs tests/invariants/cu/inv_005_terminal_oracle_role.rs
git diff --check
git show --format= --check HEAD
git diff --exit-code origin/main -- src Cargo.toml Cargo.lock tests/fixtures ':(glob)tests/invariants/*.tsv'
```

The final new selector passes: 1 test, 4 histories, 30 rollbacks, 4 closures;
peak measured CU 122,276 under the asserted 300,000 bound. A preceding green
development run measured 128,276 CU; keypair/PDA variation can change CU. This
is a measured finite bound, not a worst-case execution proof. The initial red
prevalidation used an incorrect test assumption that backing payout advances
the epoch; correcting the envelope to match the documented route semantics
made that baseline admissible. No property assertion was weakened and no
production code was changed.

Final selector output is in `new-final.log`; controls and metadata output are
in `controls.log` and `metadata.log` under the private log directory.

The adjacent command passes Lane 24 (8 worlds, 104 rollbacks, peak 55,969 CU)
and shutdown oracle ABA (8 worlds, peak 36,450 CU). All three metadata selectors
pass. Rustfmt, whitespace checks and the protected diff pass.

One existing control is red: `v16_program_cold_admin_aba_and_burn_preserve_funded_handoff_scope_and_value`
expects `InstructionError(2, Custom(8))` (`Unauthorized`) but receives
`InstructionError(2, Custom(11))` (`InvalidTokenAccount`), before SPL execution.
The only edit to that owner is the new child-module mount. The identical failure
was reproduced on the untouched base in the clean detached worktree
`/dev/shm/row416-terminal-oracle-role-20260916-basecheck`, using the same artifact
and private build environment:

```sh
cargo test --locked --offline --test v16_cu inv_005_authority_incarnation_binding::cold_admin_handoff_scope::v16_program_cold_admin_aba_and_burn_preserve_funded_handoff_scope_and_value -- --exact --nocapture --test-threads=1
```

The base reproduction is recorded in `base-control.log`. This is an existing
error-code expectation mismatch, not acceptance of stale authority or an
observed rollback violation. It is not patched in this worker, and the combined
adjacent-control command is not claimed green. Only the new green invariant
coverage, its mount and documentation are committed.
