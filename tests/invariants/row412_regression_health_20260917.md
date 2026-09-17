# Row 412 regression health, 2026-09-17

Base: freshly fetched `origin/main`, `4c3ed5627178d1e5038235d5d61444fa8f26116b`.
Worktree: `/dev/shm/percolator-row412-health-20260917`.
Branch: `row412-health-20260917`; local commit only, no push.
Primary INV-012; bounded INV-004/008/010/019/080/081 adjacency.
No production issue was confirmed; production and all status TSVs are unchanged.
The current reopening ledger says row 412 `COVERED`; older notes saying OPEN
describe earlier evidence. This increment makes no status or whole-row closure claim.

## Distinct coverage

Inputs: [README](README.md), [capability/incarnation audit](capability_incarnation_gap_audit_20260912.md),
[same-asset episode audit](retained_same_asset_episode_audit_20260912.md),
[row 412 retained-identity notes](retained_identity_replay_audit_20260912.md),
and the mounted row 412 witnesses. Only this origin/main checkout informed changes.

| Existing evidence | Added dimension |
| --- | --- |
| `retained_same_asset_episode`, `revocation_atomicity`, `mixed_batch_revocation` | Retain the owner-signed grant-writing instruction itself before partial reduction/direct flip, not only consumer requests. |
| `generated_grant_bindings` | Bilateral automatic revocation, successful writer-prefix rollback and unchanged grant delivery; the generator uses matched round trips and generation reuse. |
| `revoked_renewal_payout`, stateful `mixed_episode_renewal` | Grant signed before the revoking writer; those renewals are signed after reduction. |
| `grant_writer_order`, `retained_grant_atomicity`, `revocation_words` | Same-asset reduction/flip with retained grant admission and consumed-sequence control; existing ordered/explicit-control products do not join this history. |
| INV-019/047 `retained_mixed_transport` | Its retained grant writers land before the economic fills; this witness delivers them after rolled-back and committed revocation. |

New child: `cu/inv_012_retained_grant_episode_retry.rs`, mounted under
`revocation_atomicity`; the parent changes only by three mount lines.

Eight worlds cross single/batch bilateral writers, both signs, and partial reduction
versus direct cross-zero. Public System/SPL/ATA/wrapper setup deposits 1,000,000
atoms per owner. A CPI opens asset 2 at signed size `4q`; the owner writer applies
`-2q` or `-6q`, leaving `2q` or `-2q`. The closing CPI uses the other route.
The price stays 100, Clock slot 1, grant expiry 100, and frontier 4.

- A pre-signed grant is valid in episode 1. A seven-lamport System transfer and
  the real owner writer succeed before its old-episode grant suffix rejects
  `EngineStale`. Changing only the grant episode to 2 makes the complete bundle
  admissible in simulation. Actual rejection restores complete Accounts.
- The unchanged, separately pre-signed grant transaction then commits in episode 1.
  A second pre-signed envelope with identical grant bytes rejects its now-consumed
  sequence. This distinguishes application replay binding from transaction caching.
- The next grant and both CPI consumer routes are retained before the exact owner
  writer commits. The writer advances each episode once, disables the LP grant,
  clears expiry and leaves its grant sequence and matcher context/request count intact.
- The retained grant rejects after another successful System transfer prefix.
  Both old-episode consumers reject `EngineStale`; episode-only refreshed consumers
  reject `Unauthorized` before matcher invocation. Fresh owner consent changing only
  the grant's episode restores authority, but consumers retaining the prior grant
  sequence still reject `EngineStale` on both routes.
- A fresh CPI flattens both positions. Input-derived positions/OI, capital, zero
  PnL/insurance, grant fields, mint supply and custody reconcile through two full
  owner payouts. Portfolio IDs, asset generations, Clock and economic terms stay fixed.

All 72 refusals compare complete instruction/protected Accounts, including matcher
state, owners, SPL balances and account absence; the payer differs by exactly the
signature fee. Logs require the successful prefixes and no matcher invocation on
refusal. Signatures and the 1,232-byte packet bound are checked. No live protocol
state injection is introduced. INV-019 evidence is pre-CPI rejection/context framing,
not hostile-return validation. INV-008 evidence is consumed grant-sequence rejection.

## Results and commands

Final selector: **PASS 1/1**, 1,445 unrelated tests filtered out, 4.34s.
Eight worlds, 40 successful simulations, 72 exact rollbacks (16 restored System
transfers), eight unchanged signed grant deliveries after rollback, eight episode-only
renewals, 16 CPI fills and 16 full owner payouts. Peak measured CU: **466,880**.
The unchanged delivery is a separately retained standalone signature; the failed
bundle's transaction signature is not replayed. Transport variants differ only in
their compute-budget envelope where the same protocol request needs another delivery.

The first development run failed at the proposed active-position withdrawal prefix
with `EngineNonProgress`, before any intended grant rejection. Source inspection
also established that withdrawal requires a flat portfolio. The final witness uses
an owner-signed System transfer prefix and reserves SPL withdrawal for the flat
endpoint. This was a test setup correction, not a production fix or TDD bug claim.

Private host/SBF caches were copied, then wrapper and auth matcher rebuilt from
this checkout, locked/offline, using platform-tools v1.52. Engine pin:
`4db11a8cb0053815e23a35d3a7d3edc265d8d866`. Builds pass. Logs share prefix
`/dev/shm/percolator-row412-health-20260917-`: `sbf-build.log`, `matcher-build.log`,
`check.log` (initial setup failure), `check2.log` (final PASS).

```bash
git fetch origin main
git worktree add -b row412-health-20260917 /dev/shm/percolator-row412-health-20260917 origin/main
cd /dev/shm/percolator-row412-health-20260917
cp -a /dev/shm/percolator-public-gap-20260916-c91e-host-target /dev/shm/percolator-row412-health-20260917-host-target
cp -a /dev/shm/percolator-public-gap-20260916-c91e-sbf-target /dev/shm/percolator-row412-health-20260917-sbf-target
env CARGO_TARGET_DIR=/dev/shm/percolator-row412-health-20260917-sbf-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc CARGO_BUILD_JOBS=2 cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/percolator-row412-health-20260917-sbf-target/deploy -- --locked
env CARGO_TARGET_DIR=/dev/shm/percolator-row412-health-20260917-matcher-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc CARGO_BUILD_JOBS=2 cargo build-sbf --manifest-path tests/fixtures/auth_matcher/Cargo.toml --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir tests/fixtures/auth_matcher/target/deploy -- --locked
export CARGO_TARGET_DIR=/dev/shm/percolator-row412-health-20260917-host-target
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2
export PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-row412-health-20260917-sbf-target/deploy/percolator_prog.so
T=inv_012_capability_and_delegate_scope::joint_incarnation_binding::revocation_atomicity::retained_grant_episode_retry::v16_program_retained_grant_admission_tracks_partial_and_cross_zero_commit_or_rollback
cargo test --locked --offline --test v16_cu "$T" -- --exact --nocapture --test-threads=1
rustfmt --edition 2021 --config skip_children=true tests/invariants/cu/inv_012_retained_grant_episode_retry.rs tests/invariants/cu/inv_012_revocation_atomicity.rs
rustfmt --check --edition 2021 --config skip_children=true tests/invariants/cu/inv_012_retained_grant_episode_retry.rs tests/invariants/cu/inv_012_revocation_atomicity.rs
git diff --check
git diff --cached --check
git diff --exit-code 4c3ed5627178d1e5038235d5d61444fa8f26116b -- src Cargo.toml Cargo.lock tests/fixtures 'tests/invariants/*.tsv'
git show --format= --check HEAD
```

Scoped rustfmt, working/staged/committed whitespace checks and the unchanged
production/Cargo/fixture/status guard pass. Only the new test, its parent mount,
this report and README change. Row410/429 and row418 files are untouched.

Artifact SHA-256:
- Wrapper: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.
- Auth matcher: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.

## Open gaps

Bounded Live, one active asset, full fills, fixed price and zero fees/funding/PnL.
Arbitrary writer sequences, partial matcher fills, multi-leg/max-shape batches,
hostile retained matcher returns, context replacement after episode shifts,
grant expiry/authority rotation, Recovery/Resolved writers, durable nonce and
validator blockhash aging are outside this increment. The eight episode-only
repaired writer/grant bundles are simulations; their committed economic continuation
uses the separately submitted writer followed by a fresh grant and opposite-route exit.
